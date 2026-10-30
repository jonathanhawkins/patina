//! Visual-fidelity DOM tests for 2D/3D viewport toolbars (umbrella for pat-9bh6y).
//!
//! Complements `editor_3d_toolbar_chrome_test` with 2D toolbar assertions and a
//! combined smoke that both toolbars ship in the served document.

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

#[test]
fn viewport_toolbars_2d_and_3d_present() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        r#"id="vp3d-toolbar""#,
        r#"id="vp3d-tool-move""#,
        r#"id="vp3d-tool-rotate""#,
        r#"id="vp3d-tool-scale""#,
        r#"id="vp3d-snap""#,
        r#"id="vp2d-toolbar""#,
        r#"id="vp2d-pan""#,
        r#"id="vp2d-grid-snap""#,
        r#"id="vp2d-local-space""#,
    ] {
        assert!(
            html.contains(needle),
            "viewport toolbar missing chrome `{needle}`"
        );
    }
    handle.stop();
}

/// pat-pq0l1: the 2D/3D viewport toolbars ship Godot's *permanent* chrome.
///
/// Godot's `CanvasItemEditor` opens `main_menu_hbox` with four transform tool
/// toggles — select (pressed by default), a separator, then move / rotate /
/// scale (upstream/godot/editor/scene/canvas_item_editor_plugin.cpp:5702-5745)
/// — before the list-select / pivot / pan / ruler mode tools. Patina's
/// `#vp2d-toolbar` started at list-select and put Q/W/E/R in a non-Godot global
/// `#toolbar` instead, so the 2D viewport toolbar was missing its first four
/// buttons. The two View menus were also short of Godot 4.6.1: the 2D popup
/// (:5905-5968) replaced "Show Group And Lock Icons" with a Gizmos submenu and
/// adds Clear Guides / Auto Resample CanvasItems / Preview Theme / Preview
/// Translation, and the 3D `view_layout_menu`
/// (upstream/godot/editor/scene/3d/node_3d_editor_plugin.cpp:9999-10026) lists
/// the two Alt viewport splits plus Gizmos and Preview Translation.
#[test]
fn viewport_toolbar_transform_tools_and_view_menus_match_godot() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    handle.stop();

    // --- the four transform toggles live INSIDE the 2D viewport toolbar ---
    let tb_start = html
        .find(r#"id="vp2d-toolbar""#)
        .expect("served document has no 2D viewport toolbar");
    let tb_end = html[tb_start..]
        .find(r#"id="vp3d-view-menu""#)
        .map(|o| tb_start + o)
        .expect("2D toolbar is not followed by the 3D view menu block");
    let toolbar = &html[tb_start..tb_end];

    for needle in [
        r#"id="vp2d-tool-select""#,
        r#"id="vp2d-tool-move""#,
        r#"id="vp2d-tool-rotate""#,
        r#"id="vp2d-tool-scale""#,
        "Select Mode (Q)",
        "Move Mode (W)",
        "Rotate Mode (E)",
        "Scale Mode (R)",
        "Shift: Scale proportionally.",
    ] {
        assert!(
            toolbar.contains(needle),
            "#vp2d-toolbar is missing Godot's transform tool chrome `{needle}` \
             (canvas_item_editor_plugin.cpp:5702-5745)"
        );
    }

    // Godot order: select, move, rotate, scale, then the mode tools.
    let at = |n: &str| toolbar.find(n).unwrap_or_else(|| panic!("missing {n}"));
    let (sel, mov, rot, scl, list) = (
        at(r#"id="vp2d-tool-select""#),
        at(r#"id="vp2d-tool-move""#),
        at(r#"id="vp2d-tool-rotate""#),
        at(r#"id="vp2d-tool-scale""#),
        at(r#"id="vp2d-list-select""#),
    );
    assert!(
        sel < mov && mov < rot && rot < scl && scl < list,
        "2D toolbar must read select, move, rotate, scale, then list-select \
         (got offsets {sel}/{mov}/{rot}/{scl}/{list})"
    );

    // Select is the pressed default (`select_button->set_pressed(true)`), and
    // all four join the one exclusive tool group (`.vp2d-tool`), so clicking a
    // mode tool releases them exactly as `_button_tool_select` does.
    assert!(
        toolbar.contains(
            r#"<button class="vp2d-btn vp2d-tool active" data-vp2dtool="select" id="vp2d-tool-select""#
        ),
        "the Select tool must ship pressed and inside the exclusive .vp2d-tool group"
    );
    for tool in ["move", "rotate", "scale"] {
        assert!(
            toolbar.contains(&format!(
                r#"<button class="vp2d-btn vp2d-tool" data-vp2dtool="{tool}""#
            )),
            "the {tool} tool must join the exclusive .vp2d-tool group unpressed"
        );
    }

    // --- 2D View menu: Godot 4.6.1 item set ---
    for needle in [
        "'Show When Snapping'",
        "'Gizmos'",
        "'Transformation'",
        "'Clear Guides'",
        "'Auto Resample CanvasItems'",
        "'Preview Theme'",
        "'Preview Translation'",
    ] {
        assert!(
            html.contains(needle),
            "the 2D View menu is missing Godot item {needle} \
             (canvas_item_editor_plugin.cpp:5905-5968)"
        );
    }
    assert!(
        !html.contains("Show Group And Lock Icons"),
        "Godot 4.6 replaced \"Show Group And Lock Icons\" with the Gizmos submenu"
    );

    // --- 3D view_layout_menu: the Alt splits, Gizmos, Preview Translation ---
    for needle in [
        "2 Viewports (Alt)",
        "3 Viewports (Alt)",
        r#"data-act="view-gizmos""#,
        r#"data-act="view-preview-translation""#,
    ] {
        assert!(
            html.contains(needle),
            "the 3D View menu is missing `{needle}` \
             (node_3d_editor_plugin.cpp:9999-10026)"
        );
    }
}
