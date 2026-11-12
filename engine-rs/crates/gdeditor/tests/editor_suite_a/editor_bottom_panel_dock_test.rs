//! Godot bottom-panel docking parity (editor/gui/editor_bottom_panel.cpp).
//!
//! Godot's `EditorBottomPanel` is a tab container with
//! `set_tabs_position(POSITION_BOTTOM)` and `set_deselect_enabled(true)`: the
//! tab strip is docked at the very bottom edge, the active panel's content fills
//! the space *above* it, and clicking the open tab (or having no tab selected)
//! collapses the panel down to just the strip. The right side of the strip
//! carries a faded version label plus the Pin and Expand buttons, which only
//! show while a panel is open (editor_bottom_panel.cpp:122-123).
//!
//! Patina previously rendered the strip as a *header* above the content, so an
//! opened panel (e.g. the Audio bus mixer) overflowed off the bottom of the
//! window. These assertions lock in the inverted, Godot-faithful layout.

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
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
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

/// The tab strip is docked at the bottom edge with content ordered above it.
#[test]
fn bottom_panel_strip_is_docked_below_content() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // The content area is flex-order 0 (top) and the tab strip is order 1
    // (bottom) — Godot's POSITION_BOTTOM, achieved without reordering the DOM.
    assert!(
        html.contains("#bottom-panel-content { order: 0; flex: 1;"),
        "bottom-panel content must be flex-order 0 (above the strip)"
    );
    assert!(
        html.contains("order: 1; display: flex; align-items: center; gap: 2px; padding: 0 4px;"),
        "bottom-panel-header (tab strip) must be flex-order 1 (docked at the bottom)"
    );
    // The strip's separator is on top (it sits below the content), not below.
    assert!(
        html.contains("border-top: 1px solid var(--border); flex-shrink: 0; height: 30px;"),
        "tab strip must have a top border (it is the bottom-most row)"
    );

    handle.stop();
}

/// The panel boots collapsed (Godot `get_current_tab() == -1`) and the collapsed
/// state hides the content so only the 30px strip shows at the bottom edge.
#[test]
fn bottom_panel_starts_collapsed_and_hides_content() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    assert!(
        html.contains(r#"id="bottom-panel" class="collapsed""#),
        "bottom panel must boot collapsed (only the tab strip visible)"
    );
    assert!(
        html.contains("#bottom-panel.collapsed #bottom-panel-content { display: none; }"),
        "collapsing the panel must hide the content area, leaving just the strip"
    );

    handle.stop();
}

/// Clicking the already-open tab collapses the panel (Godot
/// `set_deselect_enabled(true)`), and auto-open triggers honor the Pin lock.
#[test]
fn bottom_panel_toggle_and_lock_behaviour_present() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    assert!(
        html.contains(
            "this.classList.contains('active') && !panel.classList.contains('collapsed')"
        ),
        "clicking the open tab must collapse the panel (deselect-enabled parity)"
    );
    // Programmatic (auto) opens route through a lock-aware helper so the Pin
    // button can block involuntary tab switches (Godot make_item_visible lock).
    assert!(
        html.contains("window.bottomPanelOpen = function"),
        "a lock-aware programmatic open helper must exist"
    );
    assert!(
        html.contains("isAuto && window.bottomPanelLocked"),
        "auto-open must early-out when the Pin lock is engaged"
    );

    handle.stop();
}

/// The right side of the strip carries the version label plus the Pin and
/// Expand buttons (Godot bottom_hbox: version_btn, pin_button, expand_button).
#[test]
fn bottom_panel_strip_has_version_pin_and_expand() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    assert!(
        html.contains(r#"id="bottom-strip-right""#),
        "strip must have a right-side cluster"
    );
    assert!(
        html.contains(r#"id="bottom-version""#),
        "strip must have a version label"
    );
    assert!(
        html.contains(r#"id="bottom-pin""#) && html.contains("Pin Bottom Panel Switching"),
        "strip must have a Pin button"
    );
    assert!(
        html.contains(r#"id="bottom-expand""#) && html.contains("Expand Bottom Panel"),
        "strip must have an Expand button"
    );
    // Pin/Expand only show while a panel is open (editor_bottom_panel.cpp:122).
    assert!(
        html.contains("#bottom-panel.collapsed .bottom-strip-btn { display: none; }"),
        "Pin/Expand must hide while the panel is collapsed"
    );

    handle.stop();
}
