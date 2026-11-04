//! pat-vyko1: the FileSystem dock's Import panel for each resource type.
//!
//! Rewritten 2026-11-03 against the route as it actually ships, and against
//! GODOT'S OWN importer contract (Godot 4.6 `ResourceImporter*`):
//!
//! - importer ids are Godot's: `texture`, `wav`, `oggvorbisstr` (NOT the
//!   invented `ogg_vorbis`), `font_data_dynamic`, `scene`;
//! - option keys are Godot 4's `[params]` keys (`compress/mode`,
//!   `mipmaps/generate`, `force/mono`, …). The old file asserted a mix of
//!   Godot 3 keys (`flags/filter`) and a bulk `params` POST that never existed;
//! - an enum is stored as its INDEX, as Godot writes it (`compress/mode=1`).
//!
//! Covers: GET defaults per extension, the empty state, POST writing a
//! well-formed `.import`, and a set → get round trip.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn http(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http(port, &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"))
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn json(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

/// A server re-rooted at a fresh temp project, so `res://` paths resolve
/// inside the sandbox (an absolute path outside it is correctly refused).
fn server() -> (EditorServerHandle, u16, PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "patina_import_panel_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.godot"), "[application]\nconfig/name=\"ImportPanel\"\n").unwrap();
    for f in ["icon.png", "effect.wav", "music.ogg", "font.ttf", "model.glb"] {
        std::fs::write(dir.join(f), b"stub").unwrap();
    }
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(150));
    let open = http_post(
        port,
        "/api/project/open",
        &serde_json::json!({ "path": dir.display().to_string() }).to_string(),
    );
    assert!(open.contains("200 OK"), "project/open must succeed: {open}");
    (handle, port, dir)
}

fn option<'a>(v: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    v["options"]
        .as_array()
        .unwrap_or_else(|| panic!("no options in {v}"))
        .iter()
        .find(|o| o["name"] == name)
        .unwrap_or_else(|| panic!("no `{name}` option in {v}"))
}

fn defaults(port: u16, file: &str) -> serde_json::Value {
    json(&http_get(port, &format!("/api/import_settings?path=res://{file}")))
}

// ===========================================================================
// 1. GET — Godot's importer and option defaults per extension
// ===========================================================================

#[test]
fn import_settings_missing_path_returns_the_empty_state() {
    let (h, port, _) = server();
    let v = json(&http_get(port, "/api/import_settings"));
    assert_eq!(v["importable"], false, "no path = the dock's empty state: {v}");
    h.stop();
}

#[test]
fn import_settings_png_returns_texture_defaults() {
    let (h, port, _) = server();
    let v = defaults(port, "icon.png");
    assert_eq!(v["importer"], "texture");
    let mode = option(&v, "compress/mode");
    assert_eq!(mode["type"], "enum");
    assert_eq!(mode["value"], "Lossless", "auto-detect preset defaults to Lossless");
    assert_eq!(option(&v, "mipmaps/generate")["value"], false);
    assert_eq!(option(&v, "process/fix_alpha_border")["value"], true);
    h.stop();
}

#[test]
fn import_settings_wav_returns_audio_defaults() {
    let (h, port, _) = server();
    let v = defaults(port, "effect.wav");
    assert_eq!(v["importer"], "wav");
    for k in ["force/8_bit", "force/mono", "edit/trim", "edit/normalize"] {
        assert_eq!(option(&v, k)["value"], false, "{k}");
    }
    assert_eq!(option(&v, "compress/mode")["value"], "Quite OK Audio");
    h.stop();
}

#[test]
fn import_settings_ogg_returns_vorbis_defaults() {
    let (h, port, _) = server();
    let v = defaults(port, "music.ogg");
    assert_eq!(v["importer"], "oggvorbisstr", "Godot's id, not `ogg_vorbis`");
    assert_eq!(option(&v, "loop")["value"], false);
    assert!(option(&v, "loop_offset")["value"].is_number());
    assert_eq!(option(&v, "bar_beats")["value"], 4);
    h.stop();
}

#[test]
fn import_settings_ttf_returns_font_defaults() {
    let (h, port, _) = server();
    let v = defaults(port, "font.ttf");
    assert_eq!(v["importer"], "font_data_dynamic");
    assert_eq!(option(&v, "antialiasing")["value"], "Grayscale");
    assert_eq!(option(&v, "hinting")["value"], "Light");
    h.stop();
}

#[test]
fn import_settings_glb_returns_scene_defaults() {
    let (h, port, _) = server();
    let v = defaults(port, "model.glb");
    assert_eq!(v["importer"], "scene");
    assert_eq!(option(&v, "animation/import")["value"], true);
    assert!(option(&v, "animation/fps")["value"].is_number());
    h.stop();
}

#[test]
fn import_settings_unknown_ext_is_not_importable() {
    let (h, port, _) = server();
    let v = defaults(port, "data.bin");
    assert_eq!(v["importable"], false);
    assert!(v.get("options").is_none(), "nothing to edit: {v}");
    h.stop();
}

// ===========================================================================
// 2. POST — writes a well-formed `.import`
// ===========================================================================

#[test]
fn set_import_settings_missing_path_returns_400() {
    let (h, port, _) = server();
    let resp = http_post(port, "/api/import_settings", r#"{"option":"loop","value":true}"#);
    assert!(resp.contains("400"), "{resp}");
    h.stop();
}

#[test]
fn set_import_settings_missing_option_returns_400() {
    let (h, port, _) = server();
    let resp = http_post(port, "/api/import_settings", r#"{"path":"res://icon.png"}"#);
    assert!(resp.contains("400"), "{resp}");
    h.stop();
}

#[test]
fn set_import_settings_writes_sidecar() {
    let (h, port, dir) = server();
    let v = json(&http_post(
        port,
        "/api/import_settings",
        r#"{"path":"res://icon.png","option":"compress/mode","value":"Lossy"}"#,
    ));
    assert_eq!(v["ok"], true, "{v}");
    let text = std::fs::read_to_string(dir.join("icon.png.import")).expect(".import written");
    assert!(text.contains("[remap]") && text.contains("[deps]") && text.contains("[params]"), "{text}");
    assert!(text.contains("importer=\"texture\""), "{text}");
    // Godot stores an enum as its index: Lossless=0, Lossy=1.
    assert!(text.contains("compress/mode=1"), "{text}");
    h.stop();
}

// ===========================================================================
// 3. Round trip — what was set is what the dock shows
// ===========================================================================

#[test]
fn import_settings_roundtrip() {
    let (h, port, _) = server();
    for k in ["force/mono", "edit/normalize"] {
        let body = format!(r#"{{"path":"res://effect.wav","option":"{k}","value":true}}"#);
        assert_eq!(json(&http_post(port, "/api/import_settings", &body))["ok"], true);
    }
    let v = defaults(port, "effect.wav");
    assert_eq!(v["importer"], "wav");
    assert_eq!(option(&v, "force/mono")["value"], true);
    assert_eq!(option(&v, "edit/normalize")["value"], true);
    // Edge: an option never set keeps Godot's default.
    assert_eq!(option(&v, "edit/trim")["value"], false);
    h.stop();
}
