//! Visual-fidelity (DOM-structure) test for the Import dock tab.
//!
//! Godot's left dock is a "Scene | Import" tab strip — the Import dock
//! (`upstream/godot/editor/docks/import_dock.cpp`) is always present as a tab,
//! showing import settings for the selected resource (or an empty-state label
//! when nothing is selected). Patina was missing the Import tab entirely.
//!
//! This asserts the served editor renders the Scene/Import tab strip, the
//! Import dock container, its empty-state label, and the import-settings
//! skeleton (Import As / Preset / options / Reimport) enumerated 1:1 from the
//! Godot constructor (import_dock.cpp:747–835).

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

/// The left dock must show a "Scene | Import" tab strip (Godot parity), not just
/// a lone "Scene" tab.
#[test]
fn scene_dock_renders_scene_and_import_tabs() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"scene-dock-tab\"",       // the Scene tab
        ">Scene<",                     // its label
        "id=\"import-dock-tab\"",      // the Import tab (was missing)
        ">Import<",                    // its label
        "function setupSceneDockTabs", // wired
        "setupSceneDockTabs()",        // invoked at boot
    ] {
        assert!(
            html.contains(needle),
            "served left dock is missing Scene/Import tab chrome `{needle}`"
        );
    }
    handle.stop();
}

/// The Import dock content matches Godot's import_dock.cpp skeleton: an
/// empty-state label plus the Import As / Preset / options / Reimport controls.
#[test]
fn import_dock_renders_skeleton() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"import-dock\"",  // the dock container
        "id=\"import-empty\"", // empty-state placeholder
        // pat-93pfv: Godot's `select_a_resource` prompt verbatim
        // (import_dock.cpp:829); Patina used to paraphrase it.
        "in the filesystem or in the inspector to adjust import settings",
        "Import As:",             // import_dock.cpp:765 OptionButton label
        "id=\"import-as\"",       // the type selector
        "id=\"import-preset\"",   // import_dock.cpp:776 preset menu
        "id=\"import-reimport\"", // import_dock.cpp:798 Reimport button
        ">Reimport<",             // verbatim label
    ] {
        assert!(
            html.contains(needle),
            "served Import dock is missing skeleton element `{needle}`"
        );
    }
    handle.stop();
}

// --- pat-h5s93: the dock populates from the selected resource (drive HTTP) ---

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

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn json_ok(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

/// pat-h5s93: a selected importable resource populates Import As / presets /
/// options and reimports; nothing / a non-importable file is the empty-state.
#[test]
fn import_dock_populates_for_selected_resource() {
    let (handle, port) = make_server();

    // A texture resource: importer id `texture`, shown as "Texture2D", with
    // presets + editable options.
    let tex = json_ok(&http_get(port, "/api/import_settings?path=res://icon.png"));
    assert_eq!(tex["importable"], serde_json::json!(true));
    // ⚠️ TWO fields, because upstream uses two. `ImportDock::_update_options`
    // pairs `get_visible_name()` as the item LABEL with `get_importer_name()`
    // as the item METADATA it acts on (editor/docks/import_dock.cpp:133,141-142),
    // so the API has to carry both: a `.import` file stores the id, the dock
    // displays the name.
    assert_eq!(
        tex["importer"],
        serde_json::json!("texture"),
        "the API carries Godot's importer ID, which is what a .import file stores"
    );
    assert_eq!(
        tex["importer_visible_name"],
        serde_json::json!("Texture2D"),
        "...and the label the dock shows in Import As"
    );
    let presets = tex["presets"].as_array().unwrap();
    assert!(!presets.is_empty(), "Preset menu is populated");
    let opt_names: Vec<String> = tex["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        opt_names.contains(&"compress/mode".to_string()),
        "texture options include compress/mode: {opt_names:?}"
    );
    assert!(
        opt_names.contains(&"mipmaps/generate".to_string()),
        "texture options include mipmaps/generate: {opt_names:?}"
    );
    let comp = tex["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "compress/mode")
        .unwrap();
    assert_eq!(comp["type"], serde_json::json!("enum"));
    assert!(
        comp["choices"].as_array().unwrap().len() >= 2,
        "compress/mode is a choice list"
    );

    // A different resource type maps to a different importer.
    let audio = json_ok(&http_get(port, "/api/import_settings?path=res://music.ogg"));
    // ⚠️ `oggvorbisstr` / "Ogg Vorbis", not "Audio Stream". That old label was
    // never Godot's: `ResourceImporterOggVorbis::get_visible_name()` returns
    // "Ogg Vorbis" (modules/vorbis/resource_importer_ogg_vorbis.cpp). It read
    // plausibly, which is exactly why it survived.
    assert_eq!(audio["importer"], serde_json::json!("oggvorbisstr"));
    assert_eq!(
        audio["importer_visible_name"],
        serde_json::json!("Ogg Vorbis")
    );

    // No path / an unimportable extension -> empty-state.
    assert_eq!(
        json_ok(&http_get(port, "/api/import_settings"))["importable"],
        serde_json::json!(false)
    );
    assert_eq!(
        json_ok(&http_get(port, "/api/import_settings?path=res://notes.xyz"))["importable"],
        serde_json::json!(false)
    );

    // Reimport works on an importable resource and rejects an unimportable one.
    let re = json_ok(&http_post(
        port,
        "/api/reimport",
        r#"{"path":"res://icon.png"}"#,
    ));
    assert_eq!(re["ok"], serde_json::json!(true));
    assert!(re["reimported"].as_str().unwrap().ends_with("icon.png"));
    assert!(
        !http_post(port, "/api/reimport", r#"{"path":"res://notes.xyz"}"#).contains("200 OK"),
        "reimport should reject a non-importable resource"
    );

    handle.stop();
}
