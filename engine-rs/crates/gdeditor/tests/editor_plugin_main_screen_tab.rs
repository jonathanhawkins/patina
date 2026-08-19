//! pat-s3rrv.66 item (f): a plugin's main screen is reachable.
//!
//! The server could already ANSWER `_has_main_screen()` and `_get_plugin_name()`
//! — `GET /api/plugins` reports both — but nothing rendered a tab and no route
//! could activate one, so `_make_visible` was reachable from exactly one place:
//! the selection handshake. A plugin could declare a main screen and never be
//! opened. That is the same decorative-control shape as the onion-skin button
//! in pat-s3rrv.62: the UI implies a feature the wiring does not deliver.
//!
//! Godot's main-screen bar shows ONE editor at a time — selecting a plugin's
//! tab calls `_make_visible(true)` on it and `_make_visible(false)` on the one
//! it replaced (`editor/plugins/editor_plugin.cpp`).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, SceneTree};
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

fn http_request(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// A plugin that owns a main screen and announces every visibility change.
fn screen_plugin(tag: &str) -> String {
    format!(
        "extends EditorPlugin\n\
         func _has_main_screen():\n\
         \x20   return true\n\
         func _make_visible(v):\n\
         \x20   if v == true:\n\
         \x20       print(\"{tag} visible true\")\n\
         \x20   if v == false:\n\
         \x20       print(\"{tag} visible false\")\n"
    )
}

/// A dock plugin: no main screen, but it DOES implement `_make_visible`, which
/// is what makes it a meaningful control in the test below.
fn dock_plugin() -> String {
    "extends EditorPlugin\n\
     func _has_main_screen():\n\
     \x20   return false\n\
     func _make_visible(v):\n\
     \x20   print(\"DOCK toggled\")\n"
        .to_string()
}

fn serve(plugins: &[(&str, String)]) -> (EditorServerHandle, u16) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut state = EditorState::new(tree);
    for (folder, src) in plugins {
        let instance = GDScriptNodeInstance::from_source(src, root).expect("plugin script parses");
        state.active_plugins.insert((*folder).to_string(), instance);
    }
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// **The acceptance case**: activating one main screen shows it and hides the
/// other.
#[test]
fn selecting_a_main_screen_shows_it_and_hides_the_previous() {
    let (handle, port) = serve(&[
        ("alpha", screen_plugin("ALPHA")),
        ("beta", screen_plugin("BETA")),
    ]);

    let resp = http_post(port, "/api/plugins/main_screen", r#"{"folder":"alpha"}"#);
    assert!(resp.contains("200 OK"), "activation answered: {resp}");

    let logs = http_get(port, "/api/logs");
    assert!(
        logs.contains("ALPHA visible true"),
        "the selected plugin must be made visible; logs: {logs}"
    );
    assert!(
        logs.contains("BETA visible false"),
        "…and the main screen it replaced must be hidden — Godot shows one at a \
         time; logs: {logs}"
    );

    handle.stop();
}

/// 🔑 A dock plugin is NOT toggled by a main-screen switch.
///
/// Its visibility is owned by the SELECTION handshake
/// (`notify_plugins_of_selection`). Hiding every plugin here would blank a dock
/// that a selection had just shown — two features fighting over one virtual.
/// This is the assertion that pins that decision; without it the obvious
/// implementation (hide everything, show one) passes the case above.
#[test]
fn a_dock_plugin_is_untouched_by_a_main_screen_switch() {
    let (handle, port) = serve(&[("alpha", screen_plugin("ALPHA")), ("dock", dock_plugin())]);

    let resp = http_post(port, "/api/plugins/main_screen", r#"{"folder":"alpha"}"#);
    assert!(resp.contains("200 OK"), "activation answered: {resp}");

    let logs = http_get(port, "/api/logs");
    assert!(
        logs.contains("ALPHA visible true"),
        "the main-screen plugin must still be shown; logs: {logs}"
    );
    assert!(
        !logs.contains("DOCK toggled"),
        "a plugin with no main screen must not have its visibility changed by a \
         main-screen switch; logs: {logs}"
    );

    handle.stop();
}

/// Asking for a plugin that has no main screen is refused with a reason, rather
/// than silently doing nothing.
#[test]
fn a_plugin_without_a_main_screen_is_refused() {
    let (handle, port) = serve(&[("dock", dock_plugin())]);

    let resp = http_post(port, "/api/plugins/main_screen", r#"{"folder":"dock"}"#);
    assert!(
        resp.starts_with("HTTP/1.1 400"),
        "a plugin with no main screen must be refused: {resp}"
    );
    assert!(
        resp.contains("no main screen"),
        "…naming the reason: {resp}"
    );

    let resp = http_post(port, "/api/plugins/main_screen", r#"{"folder":"nope"}"#);
    assert!(
        resp.starts_with("HTTP/1.1 404"),
        "an unknown plugin folder must 404: {resp}"
    );

    handle.stop();
}

/// The served shell renders a tab and routes it back to the endpoint.
///
/// ⚠️ Both halves. A tab that renders without calling the endpoint is the
/// decorative control this item exists to remove, and an endpoint no tab
/// reaches is unusable from the editor.
#[test]
fn the_shell_renders_a_main_screen_tab_that_activates_the_plugin() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    assert!(
        html.contains("refreshPluginMainScreens"),
        "the shell must build the plugin main-screen tabs"
    );
    assert!(
        html.contains("refreshPluginMainScreens();"),
        "…and actually call it during setup — a builder nothing invokes renders \
         no tab"
    );
    assert!(
        html.contains("/api/plugins/main_screen"),
        "clicking a tab must activate the plugin through the server, which is \
         what reaches _make_visible"
    );
    assert!(
        html.contains("plugin-screen"),
        "the tabs need their own class so a refresh can replace them"
    );
}
