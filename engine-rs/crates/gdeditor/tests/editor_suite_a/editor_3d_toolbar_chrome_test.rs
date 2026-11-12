//! Visual-fidelity (DOM-structure) tests for the 3D spatial editor toolbar.
//!
//! Godot's `Node3DEditor` builds a permanent toolbar in its constructor
//! (`upstream/godot/editor/scene/3d/node_3d_editor_plugin.cpp`, the
//! `main_menu_hbox`): the transform tools (Transform/Move/Rotate/Scale/Select),
//! a list-select button, lock/unlock + group/ungroup, a ruler, the
//! local-space + snap option toggles, the sun + environment preview toggles
//! with their settings button, and the Transform and View menus. None of it is
//! conditional — it is static chrome, so a user comparing the two editors in 3D
//! mode immediately sees anything missing.
//!
//! These assertions are enumerated 1:1 from the Godot source: every widget the
//! toolbar always instantiates must have a counterpart in Patina's served
//! `/editor` document. They boot the real `EditorServerHandle` and inspect the
//! served HTML — the methodology in
//! `prd/EDITOR_PARITY_VISUAL_FIDELITY_METHODOLOGY.md`.

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
    tree.add_child(root, Node::new("World", "Node3D")).unwrap();
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

/// The toolbar container itself is always present (shown only in 3D mode, but
/// rendered into the document unconditionally like Godot's `main_menu_hbox`).
#[test]
fn renders_spatial_toolbar_container() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    assert!(
        html.contains(r#"id="vp3d-toolbar""#),
        "3D spatial toolbar container missing"
    );
    handle.stop();
}

/// All five transform tools (Godot: TOOL_MODE_TRANSFORM/MOVE/ROTATE/SCALE/SELECT)
/// are present with their Q/W/E/R/V shortcuts in the tooltip.
#[test]
fn renders_transform_tools() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for (id, tip) in [
        ("vp3d-tool-transform", "Transform Mode (Q)"),
        ("vp3d-tool-move", "Move Mode (W)"),
        ("vp3d-tool-rotate", "Rotate Mode (E)"),
        ("vp3d-tool-scale", "Scale Mode (R)"),
        ("vp3d-tool-select", "Select Mode (V)"),
    ] {
        assert!(
            html.contains(&format!(r#"id="{id}""#)),
            "missing tool button {id}"
        );
        assert!(html.contains(tip), "missing tooltip for {id}: {tip}");
    }
    handle.stop();
}

/// List-select, lock/unlock, group/ungroup, and the ruler — all permanent
/// chrome from the Godot toolbar — are present with Godot's tooltip text.
#[test]
fn renders_select_lock_group_ruler() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for (id, tip) in [
        (
            "vp3d-list-select",
            "Show list of selectable nodes at position clicked.",
        ),
        (
            "vp3d-lock",
            "Lock selected node, preventing selection and movement.",
        ),
        (
            "vp3d-unlock",
            "Unlock selected node, allowing selection and movement.",
        ),
        ("vp3d-group", "Groups the selected node with its children."),
        (
            "vp3d-ungroup",
            "Ungroups the selected node from its children.",
        ),
        ("vp3d-ruler", "Ruler Mode (M)"),
    ] {
        assert!(
            html.contains(&format!(r#"id="{id}""#)),
            "missing button {id}"
        );
        assert!(html.contains(tip), "missing tooltip for {id}: {tip}");
    }
    handle.stop();
}

/// The local-space + snap option toggles (Godot TOOL_OPT_LOCAL_COORDS / USE_SNAP)
/// with their T / Y shortcuts.
#[test]
fn renders_option_toggles() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    assert!(html.contains(r#"id="vp3d-local-space""#));
    assert!(html.contains("Use Local Space (T)"));
    assert!(html.contains(r#"id="vp3d-snap""#));
    assert!(html.contains("Use Snap (Y)"));
    handle.stop();
}

/// The sun + environment preview toggles and the sun/environment settings
/// button (Godot `sun_button` / `environ_button` / `sun_environ_settings`),
/// with Godot's verbatim tooltips.
#[test]
fn renders_sun_environment_buttons() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    assert!(html.contains(r#"id="vp3d-sun""#));
    assert!(html.contains("Toggle preview sunlight."));
    assert!(html.contains(r#"id="vp3d-environ""#));
    assert!(html.contains("Toggle preview environment."));
    assert!(html.contains(r#"id="vp3d-sun-environ-settings""#));
    assert!(html.contains("Edit Sun and Environment settings."));
    handle.stop();
}

/// The Transform menu (Godot `transform_menu`) with its Snap Object to Floor,
/// Transform Dialog, and Configure Snap items.
#[test]
fn renders_transform_menu() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    assert!(html.contains(r#"id="vp3d-transform-menu-btn""#));
    assert!(html.contains("Snap Object to Floor"));
    assert!(html.contains("Transform Dialog..."));
    assert!(html.contains("Configure Snap..."));
    handle.stop();
}

/// The View menu (Godot `view_layout_menu`) with the viewport-count options,
/// View Origin, View Grid, and Settings.
#[test]
fn renders_view_menu() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    assert!(html.contains(r#"id="vp3d-viewmenu-btn""#));
    assert!(html.contains("1 Viewport"));
    assert!(html.contains("View Origin"));
    assert!(html.contains("View Grid"));
    assert!(html.contains("Settings..."));
    handle.stop();
}
