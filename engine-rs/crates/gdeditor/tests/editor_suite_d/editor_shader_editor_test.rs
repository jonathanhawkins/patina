//! Structure + behavior tests for the Shader Editor bottom panel (pat-bc0tv).
//!
//! The Shader Editor is Godot's `ShaderEditorPlugin` bottom panel
//! (`shader_editor_plugin.cpp` + `text_shader_editor.cpp`): an HSplitContainer
//! with the open-shader list (ItemList) on the left and a TextShaderEditor on
//! the right — a File/Edit/Search/Go To menu bar plus an Online Docs button, a
//! code editor with a line-number gutter and syntax highlighting, a warnings
//! panel, and a status bar. These tests assert Patina's served `/editor` HTML
//! renders that chrome (so it can't be box-ticked while a widget is absent) and
//! that the live-validation endpoint reports errors/warnings the way Godot's
//! `_validate_script` drives the shader-list error icon and warnings panel.

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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

/// The shader panel is an HSplitContainer: a shader file list on the left and
/// the TextShaderEditor on the right (Godot `files_split` in
/// `shader_editor_plugin.cpp`).
#[test]
fn shader_panel_has_files_list_and_editor_split() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    assert!(
        html.contains(r#"id="shader-panel""#),
        "shader panel present"
    );
    // Left: the open-shader ItemList.
    assert!(
        html.contains(r#"id="shader-files-list""#),
        "left shader file list (Godot shader_list ItemList)"
    );
    // A drag handle for the split (Godot HSplitContainer).
    assert!(
        html.contains(r#"id="shader-split-handle""#),
        "split resize handle"
    );
    // Right: the TextShaderEditor container.
    assert!(
        html.contains(r#"id="shader-editor-main""#),
        "TextShaderEditor container"
    );

    handle.stop();
}

/// The TextShaderEditor menu bar has File / Edit / Search / Go To menus and an
/// Online Docs button (Godot `menu_bar_hbox`: edit_menu, search_menu, goto_menu,
/// site_search). `file_menu` is attached here via `use_menu_bar`.
#[test]
fn shader_editor_has_menu_bar() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    assert!(html.contains(r#"id="shader-menubar""#), "menu bar present");
    for menu in ["file", "edit", "search", "goto"] {
        assert!(
            html.contains(&format!(r#"data-menu="{menu}""#)),
            "menu bar has the {menu} menu (Godot MenuButton)"
        );
    }
    // Godot's site_search button opens the online docs.
    assert!(
        html.contains(r#"id="shader-online-docs""#) && html.contains(">Online Docs<"),
        "Online Docs button (Godot site_search)"
    );

    // Representative menu items with their Godot labels (parity spec).
    for label in [
        "New Shader...",
        "Load Shader File...",
        "Toggle Files Panel",
        "Toggle Comment",
        "Duplicate Lines",
        "Find...",
        "Replace...",
        "Go to Line...",
        "Toggle Bookmark",
    ] {
        assert!(html.contains(label), "menu item present: {label}");
    }

    handle.stop();
}

/// The code editor has a line-number gutter, a syntax-highlight underlay, and
/// the editable textarea (Godot ShaderTextEditor / CodeTextEditor).
#[test]
fn shader_editor_has_code_editor_with_gutter_and_highlight() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    assert!(html.contains(r#"id="shader-gutter""#), "line-number gutter");
    assert!(
        html.contains(r#"id="shader-highlight""#),
        "syntax highlight underlay"
    );
    assert!(
        html.contains(r#"id="shader-source""#),
        "editable source textarea"
    );
    // Syntax token color classes (ShaderHighlightKind parity).
    assert!(html.contains(".sh-keyword"), "keyword token color");
    assert!(html.contains(".sh-type"), "builtin-type token color");

    handle.stop();
}

/// The editor has a warnings panel and a status bar with Line/Col — Godot's
/// `warnings_panel` (RichTextLabel) and the CodeTextEditor status bar.
#[test]
fn shader_editor_has_warnings_panel_and_status_bar() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    assert!(
        html.contains(r#"id="shader-warnings""#),
        "warnings panel (Godot warnings_panel)"
    );
    assert!(html.contains(r#"id="shader-status""#), "status bar");
    assert!(
        html.contains(r#"id="shader-status-linecol""#) && html.contains("Line 1, Col 1"),
        "status bar shows Line/Col"
    );
    // The panel validates live against the shader_validate endpoint.
    assert!(
        html.contains("/api/shader_validate"),
        "editor calls the shader_validate endpoint"
    );

    handle.stop();
}

/// A clean shader validates OK with no diagnostics
/// (Godot `_validate_script` → `was_compilation_successful()`).
#[test]
fn validate_endpoint_reports_clean_shader() {
    let (handle, port) = make_server();
    let body =
        r#"{"source":"shader_type canvas_item;\nvoid fragment() {\n  COLOR = vec4(1.0);\n}\n"}"#;
    let resp = http_post(port, "/api/shader_validate", body);

    assert!(
        resp.contains(r#""ok":true"#),
        "clean shader validates: {resp}"
    );
    assert!(resp.contains(r#""errors":[]"#), "no errors: {resp}");

    handle.stop();
}

/// A shader missing `shader_type` reports a fatal error (drives the shader-list
/// error icon + status bar).
#[test]
fn validate_endpoint_reports_error_for_missing_shader_type() {
    let (handle, port) = make_server();
    let body = r#"{"source":"void fragment() { COLOR = vec4(1.0); }"}"#;
    let resp = http_post(port, "/api/shader_validate", body);

    assert!(
        resp.contains(r#""ok":false"#),
        "invalid shader fails: {resp}"
    );
    assert!(
        resp.contains("shader_type"),
        "error names the cause: {resp}"
    );
    assert!(resp.contains(r#""line":1"#), "error carries a line: {resp}");

    handle.stop();
}

/// An unused uniform is a non-fatal warning surfaced in the warnings panel
/// (Godot `UNUSED_UNIFORM`).
#[test]
fn validate_endpoint_reports_unused_uniform_warning() {
    let (handle, port) = make_server();
    let body = r#"{"source":"shader_type canvas_item;\nuniform vec4 tint : source_color;\nvoid fragment() {\n  COLOR = vec4(1.0);\n}\n"}"#;
    let resp = http_post(port, "/api/shader_validate", body);

    assert!(
        resp.contains(r#""ok":true"#),
        "unused uniform is non-fatal: {resp}"
    );
    assert!(resp.contains("tint"), "warning names the uniform: {resp}");
    assert!(resp.contains("never used"), "warning explains: {resp}");

    handle.stop();
}
